import assert from 'node:assert/strict'
import { it } from 'node:test'
import { BrewSyncConflictError } from '../../../utils/brewSyncConflict'
import { ProgressSync } from './progressSync'

it('flushes the final unsent position and confirms only successful writes', async () => {
  const owner = new AbortController()
  const sent: number[] = []
  let fail = true
  const sync = new ProgressSync(async (value) => {
    sent.push(value)
    if (fail) throw new Error('offline')
  }, owner.signal)
  sync.record(31)
  sync.record(37)
  await sync.flush()
  fail = false
  await sync.flush()
  await sync.flush()
  assert.deepEqual(sent, [37, 37])
  owner.abort()
})

it('retains progress recorded during an in-flight write', async () => {
  const owner = new AbortController()
  const sent: number[] = []
  const { promise: held, resolve: release } = Promise.withResolvers<void>()
  const sync = new ProgressSync(async (value) => {
    sent.push(value)
    if (value === 10) await held
  }, owner.signal)
  sync.record(10)
  const first = sync.flush()
  await Promise.resolve()
  sync.record(20)
  release()
  await first
  await sync.flush()
  assert.deepEqual(sent, [10, 20])
  owner.abort()
})

it('never submits old-user progress after the subject is invalidated', async () => {
  const owner = new AbortController()
  const sync = new ProgressSync(
    async () => assert.fail('old subject write'),
    owner.signal,
  )
  sync.record(90)
  owner.abort()
  await sync.flush()
})

it('retries preserve observation time rather than claiming an old position is new', async () => {
  const owner = new AbortController()
  const times: number[] = []
  const originalNow = Date.now
  let now = 1000
  Date.now = () => now
  const sync = new ProgressSync(async (_value, observedAt) => {
    times.push(observedAt)
    if (times.length === 1) throw new Error('offline')
  }, owner.signal)
  try {
    sync.record(10)
    await sync.flush()
    now = 5000
    sync.record(10)
    await sync.flush()
    sync.record(20)
    await sync.flush()
    assert.deepEqual(times, [1000, 1000, 5000])
  } finally {
    owner.abort()
    Date.now = originalNow
  }
})

 it('pauses conflicting writes until explicit resolution without losing the latest position', async () => {
  const owner = new AbortController()
  const sent: number[] = []
  let conflict = true
  const sync = new ProgressSync(async (value) => {
    sent.push(value)
    if (conflict) throw new BrewSyncConflictError(7, 3)
  }, owner.signal)
  sync.record(31)
  await sync.flush()
  sync.record(42)
  await sync.flush()
  assert.deepEqual(sent, [31])
  conflict = false
  sync.resume()
  await sync.flush()
  assert.deepEqual(sent, [31, 42])
  owner.abort()
})

it('adopt confirms the remote position without sending the local one', async () => {
  const owner = new AbortController()
  const sent: number[] = []
  const sync = new ProgressSync(async (value) => {
    sent.push(value)
    throw new BrewSyncConflictError(7, 3)
  }, owner.signal)
  sync.record(31)
  await sync.flush()
  sync.adopt(12)
  await sync.flush()
  assert.deepEqual(sent, [31])
  owner.abort()
})
