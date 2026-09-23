import assert from 'node:assert/strict'
import test from 'node:test'
import {
  agentPanelOpenSessionCount,
  agentPanelSubmitDetail,
  attachAgentPanelOpenQueue,
  attachAgentSessionOpenQueue,
  clearQueuedAgentOpens,
  hasQueuedAgentPanelOpen,
  queueAgentPanelOpen,
  queueAgentSessionOpen,
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

test('queued panel open is consumed once by the attaching panel', () => {
  queueAgentPanelOpen({ view: 'manage', stage: 'full' })
  assert.equal(hasQueuedAgentPanelOpen(), true)
  const first = attachAgentPanelOpenQueue()
  assert.deepEqual(first.queued, { view: 'manage', stage: 'full' })
  // An attached panel listens itself; nothing may linger for a later remount.
  queueAgentPanelOpen({ view: 'messages', stage: 'overlay' })
  first.detach()
  const second = attachAgentPanelOpenQueue()
  assert.equal(second.queued, null)
  second.detach()
})

test('session open keeps its target until the engine attaches', () => {
  queueAgentSessionOpen({ detail: { sessionId: 's1', runId: 'r1' } } as unknown as Event)
  queueAgentSessionOpen({ detail: {} } as unknown as Event)
  const engine = attachAgentSessionOpenQueue()
  assert.deepEqual(engine.queued, { sessionId: 's1', runId: 'r1', taskId: undefined })
  engine.detach()
})

test('denied access discards pending opens', () => {
  queueAgentPanelOpen({ view: 'messages', stage: 'overlay' })
  queueAgentSessionOpen({ detail: { sessionId: 's2' } } as unknown as Event)
  clearQueuedAgentOpens()
  assert.equal(hasQueuedAgentPanelOpen(), false)
  const engine = attachAgentSessionOpenQueue()
  assert.equal(engine.queued, null)
  engine.detach()
})
