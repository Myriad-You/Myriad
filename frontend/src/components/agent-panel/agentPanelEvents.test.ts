import assert from 'node:assert/strict'
import test from 'node:test'
import {
  agentPanelOpenSessionCount,
  agentPanelSubmitDetail,
  queueAgentPanelOpen,
  takeQueuedAgentPanelOpen,
} from './agentPanelEvents'

test('accepted intention stays attached to an explicit Work submit', () => {
  const event = {
    detail: {
      text: 'Prepare the recovery summary',
      mode: 'work',
      intentionId: 'int_test',
    },
  } as unknown as Event

  assert.deepEqual(agentPanelSubmitDetail(event), {
    text: 'Prepare the recovery summary',
    mode: 'work',
    intentionId: 'int_test',
  })
})

test('missing mode remains backwards-compatible with Work', () => {
  const event = {
    detail: { text: 'Keep the old path' },
  } as unknown as Event

  assert.deepEqual(agentPanelSubmitDetail(event), {
    text: 'Keep the old path',
    mode: 'work',
  })
})

test('session selection carries its persisted count for the latest page', () => {
  assert.equal(agentPanelOpenSessionCount({ detail: { sessionId: 's', messageCount: 123 } } as unknown as Event), 123)
  assert.equal(agentPanelOpenSessionCount({ detail: { sessionId: 's' } } as unknown as Event), 0)
})

test('queued panel open is consumed once', () => {
  assert.equal(takeQueuedAgentPanelOpen(), null)
  queueAgentPanelOpen({ view: 'manage', stage: 'full' })
  assert.deepEqual(takeQueuedAgentPanelOpen(), { view: 'manage', stage: 'full' })
  assert.equal(takeQueuedAgentPanelOpen(), null)
})
