import type { TouchSummary } from './touchAppraisal'
import type { TouchObservation } from './touchGesture'
import assert from 'node:assert/strict'
import test from 'node:test'
import { TouchEncounter } from './touchEncounter'

const touch: TouchObservation = { id: 1, phase: 'start', gesture: 'hold', region: 'hair',
  durationMs: 700.2, repeatCount: 8, distance: 0, speed: 0, x: 0.5, y: 0.2 }

test('completed nearby contacts merge once; no coordinates, frame counts or idle gaps', t => {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'] })
  const sent: TouchSummary[] = []
  const encounter = new TouchEncounter(s => sent.push(s), Date.now)
  encounter.observe(touch)
  encounter.observe({ ...touch, phase: 'end' })
  t.mock.timers.tick(500)
  encounter.observe({ ...touch, id: 2 })
  for (let i = 0; i < 100; i++) encounter.observe({ ...touch, id: 2, phase: 'update' })
  t.mock.timers.tick(2000)
  assert.equal(sent.length, 0, 'never publish during ongoing contact')
  encounter.observe({ ...touch, id: 2, phase: 'end', gesture: 'stroke' })
  encounter.observe({ ...touch, id: 2, phase: 'end' })
  t.mock.timers.tick(700)
  assert.deepEqual(sent, [{ region: 'hair', gesture: 'stroke', durationMs: 1400, repeatCount: 2, displayedReaction: null }])
})

test('taps, short holds, cancellation and unmatched ends never trigger speech', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const sent: TouchSummary[] = []
  const encounter = new TouchEncounter(s => sent.push(s))
  encounter.observe({ ...touch, phase: 'end', durationMs: 4000 })
  encounter.observe(touch)
  encounter.observe({ ...touch, phase: 'end', gesture: 'tap', durationMs: 150 })
  t.mock.timers.tick(700)
  encounter.observe({ ...touch, id: 2 })
  encounter.observe({ ...touch, id: 2, phase: 'end' })
  t.mock.timers.tick(700)
  encounter.observe({ ...touch, id: 3 })
  encounter.observe({ ...touch, id: 3, phase: 'end', durationMs: 4000 })
  encounter.cancel()
  t.mock.timers.tick(700)
  assert.deepEqual(sent, [])
})

test('new regions do not inherit contact; completed episodes respect cooldown', t => {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'] })
  const sent: TouchSummary[] = []
  const encounter = new TouchEncounter(s => sent.push(s), Date.now)
  encounter.observe(touch)
  encounter.observe({ ...touch, phase: 'end', durationMs: 2000 })
  encounter.observe({ ...touch, id: 2, region: 'face' })
  encounter.observe({ ...touch, id: 2, region: 'face', phase: 'end' })
  t.mock.timers.tick(700)
  assert.equal(sent.length, 0)
  for (const id of [3, 4, 5]) {
    encounter.observe({ ...touch, id })
    encounter.observe({ ...touch, id, phase: 'end', durationMs: 2000 })
    t.mock.timers.tick(700)
    if (id === 4) t.mock.timers.tick(30_000)
  }
  assert.equal(sent.length, 2)
})
