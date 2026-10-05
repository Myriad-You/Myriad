import type { TestContext } from 'node:test'
import assert from 'node:assert/strict'
import test from 'node:test'
import { beginIdentityChange, settleIdentity } from '../../utils/identity'
import {
  agentPanelOpenSessionCount,
  agentPanelSubmitDetail,
  askAgentPanel,
  attachAgentPanelOpenQueue,
  attachAgentSessionOpenQueue,
  clearQueuedAgentOpens,
  hasQueuedAgentPanelOpen,
  queueAgentPanelOpen,
  queueAgentSessionOpen,
  takeQueuedAgentSubmit,
} from './agentPanelEvents'
import { getAgentPanelMode, setAgentPanelMode } from './agentPanelMode'

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

test('an identity change discards the previous subject\'s queued session open', () => {
  queueAgentSessionOpen({ detail: { sessionId: 'account-a-session' } } as unknown as Event)
  beginIdentityChange()
  settleIdentity({ id: 2 })
  const engine = attachAgentSessionOpenQueue()
  assert.equal(engine.queued, null)
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

function withWindow(t: TestContext, target: EventTarget): void {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'window')
  Object.defineProperty(globalThis, 'window', { configurable: true, value: target })
  t.after(() => {
    if (original) Object.defineProperty(globalThis, 'window', original)
    else Reflect.deleteProperty(globalThis, 'window')
  })
}

test('asking from outside the panel keeps the words until the engine can send', (t) => {
  const opened: string[] = []
  const target = new EventTarget()
  target.addEventListener('agent-panel-open', (event) => {
    opened.push((event as CustomEvent<{ view: string }>).detail.view)
  })
  withWindow(t, target)
  setAgentPanelMode('work')
  askAgentPanel('  来玩你出的这道汤：暴雪夜  ', 'chat')
  assert.equal(getAgentPanelMode(), 'chat')
  assert.deepEqual(takeQueuedAgentSubmit(), { text: '来玩你出的这道汤：暴雪夜', mode: 'chat' })
  assert.equal(takeQueuedAgentSubmit(), null)
  assert.deepEqual(opened, ['messages'])
})

test('an identity change discards words asked for the previous subject', (t) => {
  withWindow(t, new EventTarget())
  askAgentPanel('来一局', 'chat')
  beginIdentityChange()
  settleIdentity({ id: 3 })
  assert.equal(takeQueuedAgentSubmit(), null)
})
