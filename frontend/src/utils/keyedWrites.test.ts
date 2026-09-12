import assert from 'node:assert/strict'
import { it } from 'node:test'
import { KeyedWrites } from './keyedWrites'

it('orders same-article intents while allowing other articles to proceed', async () => {
  const writes = new KeyedWrites()
  const signal = new AbortController().signal
  const events: string[] = []
  const { promise: held, resolve: release } = Promise.withResolvers<void>()
  const first = writes.run('A', signal, async () => {
    events.push('A star')
    await held
    events.push('A confirmed')
  })
  const second = writes.run('A', signal, async () => { events.push('A unstar') })
  await writes.run('B', signal, async () => { events.push('B read') })
  assert.deepEqual(events, ['A star', 'B read'])
  release()
  await Promise.all([first, second])
  assert.deepEqual(events, ['A star', 'B read', 'A confirmed', 'A unstar'])
})

it('a failed write does not block the next intent and revoked queued work never runs', async () => {
  const writes = new KeyedWrites()
  const owner = new AbortController()
  const first = writes.run('A', owner.signal, async () => { throw new Error('offline') })
  const second = writes.run('A', owner.signal, async () => 42)
  await assert.rejects(first, /offline/)
  assert.equal(await second, 42)
  const cancelled = writes.run('A', owner.signal, async () => assert.fail('revoked write'))
  owner.abort()
  await assert.rejects(cancelled, { name: 'AbortError' })
})

it('bulk writes wait for earlier articles and later article intents wait for the bulk write', async () => {
  const writes = new KeyedWrites()
  const signal = new AbortController().signal
  const events: string[] = []
  const { promise: held, resolve: release } = Promise.withResolvers<void>()
  const first = writes.run('A', signal, async () => {
    events.push('A before')
    await held
  })
  const bulk = writes.barrier(signal, async () => { events.push('all read') })
  const last = writes.run('B', signal, async () => { events.push('B unread') })
  await new Promise(resolve => setTimeout(resolve, 0))
  assert.deepEqual(events, ['A before'])
  release()
  await Promise.all([first, bulk, last])
  assert.deepEqual(events, ['A before', 'all read', 'B unread'])
})
