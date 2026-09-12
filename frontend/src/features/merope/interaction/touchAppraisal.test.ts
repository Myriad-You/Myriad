import type { TouchSummary } from './touchAppraisal'
import type { TouchObservation } from './touchGesture'
import assert from 'node:assert/strict'
import test from 'node:test'
import { TouchAppraisal } from './touchAppraisal'

const touch: TouchObservation = { id: 1, phase: 'update', gesture: 'hold', region: 'hair',
  durationMs: 500, distance: 0, speed: 0, repeatCount: 1, x: 0.5, y: 0.2 }
function harness() {
  let now = 1000
  const calls: { summary: TouchSummary; signal: AbortSignal; resolve: (value: unknown) => void }[] = []
  const applied: unknown[] = []
  const client = new TouchAppraisal({ now: () => now,
    request: (summary, signal) => {
      const deferred = Promise.withResolvers<unknown>()
      calls.push({ summary, signal, resolve: deferred.resolve })
      return deferred.promise
    },
    apply: (revision, reaction) => applied.push({ revision, reaction }),
  })
  return { client, calls, applied, time: (value: number) => { now = value } }
}
const flush = () => new Promise(resolve => setImmediate(resolve))

test('cancellation frees the slot even if transport ignores abort; old completion cannot overwrite successor', async () => {
  const h = harness()
  h.client.observe(touch, 1)
  h.client.cancel()
  await flush()
  h.time(6000)
  h.client.observe(touch, 2)
  assert.equal(h.calls.length, 2)
  h.calls[0].resolve({ reaction: 'withdraw' })
  h.calls[1].resolve({ reaction: 'accept' })
  await flush()
  assert.deepEqual(h.applied, [{ revision: 2, reaction: 'accept' }])
  h.client.dispose()
})

test('only semantic sustained contact requests; no coordinates; no per-frame calls', async () => {
  const h = harness()
  h.client.observe({ ...touch, durationMs: 100, gesture: 'contact' }, 1)
  assert.equal(h.calls.length, 0)
  h.client.observe(touch, 2)
  for (let i = 0; i < 100; i++) h.client.observe(touch, 2)
  assert.equal(h.calls.length, 1)
  assert.deepEqual(Object.keys(h.calls[0].summary).toSorted(), ['durationMs', 'gesture', 'region', 'repeatCount'])
  h.calls[0].resolve({ reaction: 'accept' })
  await flush()
  assert.deepEqual(h.applied, [{ revision: 2, reaction: 'accept' }])
  h.time(7000)
  h.client.observe(touch, 2)
  assert.equal(h.calls.length, 1)
  h.client.dispose()
})

test('changed gesture drops stale decision and merges updates into latest revision after cooldown', async () => {
  const h = harness()
  h.client.observe(touch, 1)
  h.client.observe({ ...touch, gesture: 'stroke' }, 2)
  assert.equal(h.calls[0].signal.aborted, true, 'obsolete evidence should stop consuming transport time')
  h.calls[0].resolve({ reaction: 'accept' })
  await flush()
  assert.equal(h.applied.length, 0)
  h.client.observe(touch, 3)
  assert.equal(h.calls.length, 1)
  h.time(6000)
  h.client.observe({ ...touch, region: 'face' }, 4)
  assert.equal(h.calls.length, 2)
  assert.equal(h.calls[1].summary.region, 'face')
  h.calls[1].resolve({ reaction: 'hesitate' })
  await flush()
  assert.deepEqual(h.applied, [{ revision: 4, reaction: 'hesitate' }])
  h.client.dispose()
})

for (const ending of ['end', 'cancel', 'dispose', 'expired', 'malformed'] as const) {
  test(`${ending} cannot apply an obsolete or invalid result`, async () => {
    const h = harness()
    h.client.observe(touch, 1)
    if (ending === 'dispose') h.client.dispose()
    else if (ending === 'end' || ending === 'cancel') h.client.observe({ ...touch, phase: ending }, 1)
    else if (ending === 'expired') h.time(5000)
    h.calls[0].resolve({ reaction: ending === 'malformed' ? 'execute' : 'accept' })
    await flush()
    assert.equal(h.applied.length, 0)
    h.client.dispose()
  })
}
