import type { TouchObservation } from './touchGesture'
import assert from 'node:assert/strict'
import test from 'node:test'
import { RigMotionCoordinator } from '../motion/coordinator'
import { TouchMotionSource } from '../motion/touchSource'
import { resetPresenceInboundForTest, setPresenceArmedForTest,
  setPresenceCaptureForTest, setPresenceFactsForTest, setPresencePostForTest } from '../perception/inbound'
import { perceptionRegistry } from '../perception/registry'
import { createTouchAppraisal } from './touchAppraisalHost'

const start: TouchObservation = { id: 1, phase: 'start', gesture: 'contact', region: 'hair',
  durationMs: 0, repeatCount: 1, distance: 0, speed: 0, x: 0.5, y: 0.2 }
const flush = () => new Promise(resolve => setImmediate(resolve))

test('production touch host publishes perception and one completion after release; lifecycle aborts delivery', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const originals = ['document', 'sessionStorage', 'fetch'].map(key =>
    [key, Object.getOwnPropertyDescriptor(globalThis, key)] as const)
  const token = `v1.${'a'.repeat(16)}.${'b'.repeat(43)}`
  const storage = new Map([
    ['csrf_token', token], ['csrf_token_stored_at', String(Date.now())],
  ])
  Object.defineProperty(globalThis, 'document', { configurable: true,
    value: { hidden: false, visibilityState: 'visible' } })
  Object.defineProperty(globalThis, 'sessionStorage', { configurable: true,
    value: { getItem: (key: string) => storage.get(key) ?? null } })
  const requests: { url: string; init: RequestInit }[] = []
  Object.defineProperty(globalThis, 'fetch', { configurable: true,
    value: (url: string, init: RequestInit) => {
      requests.push({ url, init })
      return new Promise((_resolve, reject) => {
        init.signal?.addEventListener('abort', () => reject(new Error('cancelled')), { once: true })
      })
    } })
  resetPresenceInboundForTest()
  setPresenceArmedForTest(true)
  const presence: unknown[] = []
  setPresenceCaptureForTest(() => perceptionRegistry.active())
  setPresenceFactsForTest(() => ({ speaking: false, faceVisible: true, pageVisible: true }))
  setPresencePostForTest(async body => { presence.push(body) })
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const host = createTouchAppraisal('test', source)
  try {
    source.update('test', start, 0)
    host.observe(start, 1)
    source.update('test', { ...start, phase: 'update', gesture: 'hold', durationMs: 500 }, 500)
    source.refine('test', source.version(), 'withdraw', 500)
    source.notePresented('test', { behaviorId: source.current()!.id, reaction: 'withdraw', atMs: 650 }, 650)
    source.update('test', { ...start, phase: 'end', gesture: 'stroke', durationMs: 1500.4 }, 1500)
    host.observe({ ...start, phase: 'end', gesture: 'stroke', durationMs: 1500.4 }, 1)
    assert.equal(requests.length, 0)
    t.mock.timers.tick(700)
    await flush()
    assert.equal(requests.length, 1)
    assert.ok(requests[0].url.endsWith('/addressee/touch/complete'))
    assert.equal(requests[0].init.credentials, 'include')
    assert.deepEqual(JSON.parse(String(requests[0].init.body)), {
      region: 'hair', gesture: 'stroke', durationMs: 1500, repeatCount: 1, displayedReaction: 'withdraw',
    })
    assert.equal(presence.length, 1)
    const snapshot = perceptionRegistry.active().find(s => s.sourceId === 'avatar-touch')!
    assert.equal(snapshot.safeFacts.completed, true)
    assert.equal(snapshot.safeFacts.displayedReaction, 'withdraw')
    assert.equal(snapshot.privacy, 'local')
    assert.equal(snapshot.expiresAt - snapshot.capturedAt, 20_000)
    host.dispose()
    await flush()
    assert.equal(requests[0].init.signal?.aborted, true)
    assert.ok(!perceptionRegistry.active().some(s => s.sourceId === 'avatar-touch'))
    t.mock.timers.tick(60_000)
    assert.equal(requests.length, 1)
  } finally {
    host.dispose()
    source.release()
    resetPresenceInboundForTest()
    for (const [key, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
