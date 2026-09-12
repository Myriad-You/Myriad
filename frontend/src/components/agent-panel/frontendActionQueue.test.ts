import assert from 'node:assert/strict'
import test from 'node:test'
import { AuthSubjectScope } from '../../utils/authSubject'
import { FrontendActionQueue } from './frontendActionQueue'

test('identity change drops remaining actions, queued batches and already collected results', async () => {
  const scope = new AuthSubjectScope()
  const held = Promise.withResolvers<void>()
  const entered = Promise.withResolvers<void>()
  const calls: string[] = []
  let owner = 'A'
  const queue = new FrontendActionQueue(async (action) => {
    const result = { owner }
    calls.push(action.type)
    if (action.type === 'query_windows') {
      entered.resolve()
      await held.promise
    }
    return result
  }, 10)
  const first = queue.enqueue('old', [
    { type: 'show_data' }, { type: 'query_windows' }, { type: 'navigate' },
  ], scope.signal)
  await entered.promise
  const queued = queue.enqueue('old', [{ type: 'music_control' }], scope.signal)
  scope.change('B')
  owner = 'B'
  queue.reset()
  // B can reuse the id without waiting for A's uncooperative handler.
  assert.deepEqual(await queue.enqueue('old', [{ type: 'show_data' }], scope.signal), [{ owner: 'B' }])
  held.resolve()
  assert.deepEqual(await first, [])
  assert.deepEqual(await queued, [])
  assert.deepEqual(calls, ['show_data', 'query_windows', 'show_data'])
})

test('aborted identity is rejected before execution even without a React reset', async () => {
  const scope = new AuthSubjectScope()
  let calls = 0
  const queue = new FrontendActionQueue(async () => { calls++; return {} }, 10)
  const pending = queue.enqueue('old', [{ type: 'show_data' }], scope.signal)
  scope.change('B')
  assert.deepEqual(await pending, [])
  assert.equal(calls, 0)
})

test('same identity preserves ordering and step/final dedupe; different messages can run', async () => {
  const scope = new AuthSubjectScope()
  const held = Promise.withResolvers<void>()
  const entered = Promise.withResolvers<void>()
  const calls: string[] = []
  const queue = new FrontendActionQueue(async action => {
    calls.push(action.type)
    if (action.type === 'navigate') { entered.resolve(); await held.promise }
    return { type: action.type }
  }, 10)
  const first = queue.enqueue('one', [{ type: 'navigate', timestamp: 1 }], scope.signal)
  await entered.promise
  scope.change('guest') // Same-user refresh is not an identity transition.
  const next = queue.enqueue('one', [
    { type: 'navigate', timestamp: 1 }, { type: 'show_data' },
  ], scope.signal)
  await queue.enqueue('two', [{ type: 'music_control' }], scope.signal)
  assert.deepEqual(calls, ['navigate', 'music_control'])
  held.resolve()
  await first
  assert.deepEqual(await next, [{ type: 'show_data' }])
  assert.deepEqual(calls, ['navigate', 'music_control', 'show_data'])
})

test('engine disposal cancels pending work without changing the shared subject', async () => {
  const scope = new AuthSubjectScope()
  let calls = 0
  const queue = new FrontendActionQueue(async () => { calls++ }, 10)
  const pending = queue.enqueue('one', [{ type: 'navigate' }], scope.signal)
  queue.reset()
  await pending
  assert.equal(scope.signal.aborted, false)
  assert.equal(calls, 0)
})
