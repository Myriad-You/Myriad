import type { PerformanceDirective } from '../../../services/agent/types'
import type { TouchCompletion } from './touchEncounter'
import type { TouchObservation } from './touchGesture'
import assert from 'node:assert/strict'
import test from 'node:test'
import { realizeAnime25DBehaviorPlan } from '../anime25drig/behaviorRealizer'
import { PerformanceExpressionController } from '../anime25drig/performanceExpression'
import { RigMotionCoordinator } from '../motion/coordinator'
import { HumanPerformanceRuntime } from '../motion/humanPerformanceRuntime'
import { TouchMotionSource } from '../motion/touchSource'
import { TouchEncounter } from './touchEncounter'

const touch: TouchObservation = { id: 1, phase: 'start', region: 'hair', gesture: 'contact',
  durationMs: 0, repeatCount: 0, speed: 0, distance: 0, x: 0, y: 0 }

for (const reaction of ['accept', 'hesitate', 'withdraw'] as const) {
  for (const fps of [30, 60, 120]) { test(`released ${reaction} accompanies only its spoken line at ${fps} fps`, t => {
    t.mock.timers.enable({ apis: ['setTimeout'] })
    const coordinator = new RigMotionCoordinator()
    const source = new TouchMotionSource(coordinator, () => {})
    const scheduler = new HumanPerformanceRuntime()
    const expression = new PerformanceExpressionController()
    source.update('panel', touch, 0)
    source.update('panel', { ...touch, phase: 'update', gesture: 'hold', durationMs: 1300 }, 1300)
    source.refine('panel', source.version(), reaction, 1300)
    for (let i = 0; i <= fps; i++) {
      const now = 1300 + i * 1000 / fps
      const frame = scheduler.frame([source.current()], now)
      expression.playBehaviorUnits(frame.plan ? realizeAnime25DBehaviorPlan(frame.plan, now).units : [], now / 1000, now)
      expression.sample(now / 1000)
      const receipt = expression.getSampledTouch()
      source.notePresented('panel', receipt ? { ...receipt, atMs: now } : null, now)
    }
    assert.equal(source.displayedReaction('panel', 1), reaction)
    source.update('panel', { ...touch, phase: 'end', gesture: 'hold', durationMs: 2300 }, 2300)
    t.mock.timers.tick(700)
    assert.equal(source.current(), null)
    source.expectSpeech('panel', reaction, 3000)
    source.accompanySpeech('line', 3500)
    assert.equal(source.speechContinuation(3500, () => false), null, 'no speech means no revived pose')
    const plan = source.speechContinuation(3600, id => id === 'line')!
    const contradiction: PerformanceDirective = { phase: 'delivery', moodRevision: 1, motionStyle: 'even',
      plan: { cues: [{ intent: 'silly', atMs: 0, intensity: 1, tempo: 1, fadeInMs: 80, fadeOutMs: 300, interrupt: 'replace' }] } }
    assert.equal(source.acceptsSpeechRefinement('line', contradiction, 3600), reaction === 'accept')
    assert.equal(source.acceptsSpeechRefinement('other-line', contradiction, 3600), true)
    assert.equal(source.acceptsSpeechRefinement('line', { ...contradiction, plan: { cues: [] },
      phrases: [{ text: '先别碰我。', intent: 'none' }] }, 3600), true)
    assert.equal(source.acceptsSpeechRefinement('line', { ...contradiction, plan: { cues: [] },
      phrases: [{ text: '先别碰我。', intent: 'tease' }] }, 3600), reaction === 'accept')
    assert.equal(plan.behaviors[0].form.id, reaction)
    assert.equal(plan.originMs, 0, 'do not replay the orienting stroke')
    assert.equal(plan.behaviors[0].form.parameters?.strokeX, 0)
    assert.equal(plan.behaviors[0].form.parameters?.caress, 0)
    for (let i = 0; i <= fps; i++) {
      const now = 3600 + i * 1000 / fps
      const frame = scheduler.frame([source.speechContinuation(now, id => id === 'line')], now)
      expression.playBehaviorUnits(frame.plan ? realizeAnime25DBehaviorPlan(frame.plan, now).units : [], now / 1000, now)
      const sample = expression.sample(now / 1000)
      assert.ok(Object.values(sample).every(Number.isFinite))
    }
    assert.equal(expression.getSampledTouch()?.reaction, reaction)
    assert.equal(source.speechContinuation(4700, () => false), null, 'finish or new conversation retires the attitude')
    assert.equal(source.speechContinuation(4800, () => true), null, 'late director cannot revive it')
    assert.equal(source.acceptsSpeechRefinement('line', contradiction, 4800), true, 'no permanent expression blacklist')
    assert.notEqual(coordinator.snapshot(4800).owners.expression, 'performance')
    for (let i = 0; i <= fps * 2; i++) {
      const now = 4800 + i * 1000 / fps
      const frame = scheduler.frame([], now)
      expression.playBehaviorUnits(frame.plan ? realizeAnime25DBehaviorPlan(frame.plan, now).units : [], now / 1000, now)
      expression.sample(now / 1000)
    }
    assert.equal(expression.getSampledTouch(), null, 'finished speech fades back instead of latching')
    source.release()
  })
}
}

test('speech cannot inherit unrendered, expired or previous-owner touch', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  source.update('panel', touch, 0)
  source.refine('panel', source.version(), 'accept', 500)
  source.accompanySpeech('unseen', 600)
  assert.equal(source.speechContinuation(600, () => true), null)
  source.notePresented('panel', { behaviorId: source.current()!.id, reaction: 'accept', atMs: 650 }, 650)
  source.update('panel', { ...touch, phase: 'end', gesture: 'hold', durationMs: 700 }, 700)
  t.mock.timers.tick(700)
  source.accompanySpeech('expired', 5000)
  assert.equal(source.speechContinuation(5000, () => true), null)
  source.release('panel')
  source.accompanySpeech('retired', 1000)
  assert.equal(source.speechContinuation(1000, () => true), null)
})

test('completion evidence is single-use and invalidated by a newer contact', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const complete = (id: number, at: number) => {
    source.update('panel', { ...touch, id }, at)
    source.refine('panel', source.version(), 'withdraw', at + 500)
    source.notePresented('panel', { behaviorId: source.current()!.id, reaction: 'withdraw', atMs: at + 600 }, at + 600)
    source.update('panel', { ...touch, id, phase: 'end', gesture: 'hold', durationMs: 1500 }, at + 1500)
    t.mock.timers.tick(700)
    source.expectSpeech('panel', 'withdraw', at + 2200)
  }
  complete(1, 0)
  source.accompanySpeech('first', 6000)
  assert.equal(source.speechContinuation(6100, () => true)?.behaviors[0].form.id, 'withdraw')
  source.speechContinuation(6200, () => false)
  source.accompanySpeech('duplicate', 6300)
  assert.equal(source.speechContinuation(6300, () => true), null)
  complete(2, 7000)
  source.update('panel', { ...touch, id: 3, region: 'face' }, 9300)
  source.accompanySpeech('old-line', 9400)
  assert.equal(source.speechContinuation(9400, () => true), null)
  source.release()
  complete(4, 10000)
  source.cancelExpectedSpeech('panel')
  source.accompanySpeech('cancelled', 13000)
  assert.equal(source.speechContinuation(13000, () => true), null)
  source.release()
})

test('releasing speech-only touch continuity notifies the renderer and releases its lease', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  let changes = 0
  const coordinator = new RigMotionCoordinator()
  const source = new TouchMotionSource(coordinator, () => { changes++ })
  source.update('panel', touch, 0)
  source.refine('panel', source.version(), 'withdraw', 500)
  source.notePresented('panel', { behaviorId: source.current()!.id, reaction: 'withdraw', atMs: 650 }, 650)
  source.update('panel', { ...touch, phase: 'end', gesture: 'hold', durationMs: 1500 }, 1500)
  t.mock.timers.tick(700)
  source.expectSpeech('panel', 'withdraw', 2200)
  source.accompanySpeech('line', 3000)
  assert.ok(source.speechContinuation(3100, () => true))
  assert.equal(source.current(), null)
  const before = changes
  source.release('panel')
  assert.equal(changes, before + 1)
  assert.equal(source.speechContinuation(3200, () => true), null)
  assert.notEqual(coordinator.snapshot(3200).owners.expression, 'performance')
  source.release('panel')
  assert.equal(changes, before + 1, 'repeated cleanup is inert')
})

for (const fps of [30, 60, 120]) { test(`sampled reaction -> summary -> next contact stays coherent at ${fps} fps`, t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const scheduler = new HumanPerformanceRuntime()
  const expression = new PerformanceExpressionController()
  const summaries: TouchCompletion[] = []
  const encounter = new TouchEncounter(s => summaries.push(s))
  const observe = (event: TouchObservation, now: number) => {
    source.update('panel', event, now)
    encounter.observe(event, source.displayedReaction('panel', event.id))
  }
  observe(touch, 0)
  assert.equal(source.displayedReaction('panel', 1), null, 'a plan is not evidence of display')
  for (let i = 1; i <= fps * 1.5; i++) {
    const now = i * 1000 / fps
    const event = { ...touch, phase: 'update' as const, durationMs: now,
      gesture: now >= 400 ? 'hold' as const : 'contact' as const }
    observe(event, now)
    if (now >= 600 && now < 600 + 1000 / fps) source.refine('panel', source.version(), 'withdraw', now)
    const frame = scheduler.frame([source.current()], now)
    expression.playBehaviorUnits(frame.plan ? realizeAnime25DBehaviorPlan(frame.plan, now).units : [], now / 1000, now)
    expression.sample(now / 1000)
    const sampled = expression.getSampledTouch()
    source.notePresented('panel', sampled ? { ...sampled, atMs: now } : null, now)
  }
  assert.equal(source.displayedReaction('panel', 1), 'withdraw')
  const oldRevision = source.version()
  source.refine('panel', oldRevision, 'accept', 1510)
  assert.equal(source.displayedReaction('panel', 1), 'withdraw')
  observe({ ...touch, phase: 'end', gesture: 'hold', durationMs: 1510 }, 1510)
  t.mock.timers.tick(700)
  assert.equal(summaries.length, 1)
  assert.equal(summaries[0].displayedReaction, 'withdraw', 'summary uses the frame, not the last requested form')
  assert.equal(source.current(), null, 'visual tail ends independently of short-term continuity')
  observe({ ...touch, id: 2 }, 2300)
  assert.equal(source.current()!.behaviors[0].form.id, 'withdraw')
  observe({ ...touch, id: 2, phase: 'update', gesture: 'stroke', durationMs: 500 }, 2800)
  source.refine('panel', oldRevision, 'accept', 2801)
  assert.equal(source.current()!.behaviors[0].form.id, 'withdraw')
  source.setAffect(20, 70, 2802)
  assert.equal(source.current()!.behaviors[0].form.id, 'hesitate', 'new mood does not inherit the previous episode blindly')
  source.release('panel')
  observe({ ...touch, id: 3 }, 3000)
  assert.equal(source.displayedReaction('panel', 3), null)
  assert.equal(source.current()!.behaviors[0].form.id, 'notice')
  source.release()
  encounter.cancel()
})
}

test('unrendered, stale and other-owner receipts cannot seed continuity; remembered response expires', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  source.update('panel', touch, 0)
  source.update('panel', { ...touch, phase: 'update', gesture: 'hold', durationMs: 500 }, 500)
  const receipt = { behaviorId: source.current()!.id, reaction: 'accept' as const, atMs: 500 }
  source.notePresented('other', receipt, 500)
  source.notePresented('panel', receipt, 900)
  assert.equal(source.displayedReaction('panel', 1), null)
  source.notePresented('panel', receipt, 500)
  source.update('panel', { ...touch, phase: 'end', gesture: 'hold', durationMs: 600 }, 600)
  t.mock.timers.tick(700)
  source.update('panel', { ...touch, id: 2 }, 1000)
  assert.equal(source.current()!.behaviors[0].form.id, 'accept', 'short pause does not replay surprise')
  source.update('panel', { ...touch, id: 2, phase: 'end', gesture: 'hold', durationMs: 500 }, 1500)
  t.mock.timers.tick(700)
  source.update('panel', { ...touch, id: 3 }, 5000)
  assert.equal(source.current()!.behaviors[0].form.id, 'notice', 'no permanent reaction latch')
  source.release()
})

test('changing region clears continuity and a retired owner cannot clear the new body', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  source.update('panel', touch, 0)
  source.update('panel', { ...touch, phase: 'update', gesture: 'hold', durationMs: 500 }, 500)
  source.refine('panel', source.version(), 'withdraw', 500)
  source.notePresented('panel', { behaviorId: source.current()!.id, reaction: 'withdraw', atMs: 650 }, 650)
  source.update('panel', { ...touch, phase: 'end', gesture: 'hold', durationMs: 700 }, 700)
  t.mock.timers.tick(700)
  source.update('panel', { ...touch, id: 2, region: 'face' }, 1500)
  source.update('panel', { ...touch, id: 2, region: 'face', phase: 'end', gesture: 'hold', durationMs: 500 }, 2000)
  source.update('widget', { ...touch, id: 3 }, 2100)
  const current = source.current()
  assert.equal(current!.behaviors[0].form.id, 'notice')
  source.release('panel')
  assert.equal(source.current(), current)
  source.release('widget')
})
