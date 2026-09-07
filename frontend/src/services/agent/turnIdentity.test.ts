import assert from 'node:assert/strict'
import test from 'node:test'
import {
  acceptRunSequence,
  ChatTurnClock,
  isCurrentChatGeneration,
  isStreamSupersededError,
  isTerminalTurnPhase,
  isUserInterruptError,
  nextAgentMessageId,
  STREAM_INTERRUPTED_MESSAGE,
  STREAM_SUPERSEDED_MESSAGE,
} from './turnIdentity'

test('ASR results committed in the same millisecond have distinct message IDs', (t) => {
  t.mock.method(Date, 'now', () => 42)
  const ids = Array.from({ length: 100 }, () => nextAgentMessageId('assistant'))
  assert.equal(new Set(ids).size, 100)
  assert.ok(ids.every((id) => id.startsWith('msg_assistant_42_')))
})

test('generation is an in-memory counter, not a second turn id', () => {
  const clock = new ChatTurnClock()
  assert.equal(clock.current(), 0)
  assert.equal(clock.next(), 1)
  assert.equal(clock.next(), 2)
  assert.equal(isCurrentChatGeneration(1, 2), false)
  assert.equal(isCurrentChatGeneration(2, 2), true)
})

test('completed cancelled superseded and failed are terminal', () => {
  assert.equal(isTerminalTurnPhase('generating'), false)
  assert.equal(isTerminalTurnPhase('speaking'), false)
  assert.equal(isTerminalTurnPhase('completed'), true)
  assert.equal(isTerminalTurnPhase('cancelled'), true)
  assert.equal(isTerminalTurnPhase('superseded'), true)
  assert.equal(isTerminalTurnPhase('failed'), true)
})

test('duplicate sequences for the same run are dropped', () => {
  const seen = new Map<string, number>()
  assert.equal(acceptRunSequence(seen, 'run_a', 1), true)
  assert.equal(acceptRunSequence(seen, 'run_a', 1), false)
  assert.equal(acceptRunSequence(seen, 'run_a', 2), true)
  assert.equal(acceptRunSequence(seen, 'run_b', 1), true)
  assert.equal(acceptRunSequence(seen, 'run_a', 2), false)
})

test('supersede errors are distinct from faults', () => {
  assert.equal(
    isStreamSupersededError(new Error(STREAM_SUPERSEDED_MESSAGE)),
    true,
  )
  assert.equal(isStreamSupersededError(new Error('Request timed out')), false)
  assert.equal(isStreamSupersededError('TURN_SUPERSEDED'), false)
  assert.equal(
    isUserInterruptError(new Error(STREAM_INTERRUPTED_MESSAGE)),
    true,
  )
  assert.equal(
    isUserInterruptError(new Error(STREAM_SUPERSEDED_MESSAGE)),
    false,
  )
})
