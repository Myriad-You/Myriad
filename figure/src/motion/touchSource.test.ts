import assert from 'node:assert/strict'
import test from 'node:test'
import { realizeAnime25DBehaviorPlan } from '../anime25drig/behaviorRealizer'
import { PerformanceExpressionController } from '../anime25drig/performanceExpression'
import { TouchAppraisal } from '../interaction/touchAppraisal'
import { TouchGestureTracker } from '../interaction/touchGesture'
import { RigMotionCoordinator } from './coordinator'
import { HumanPerformanceRuntime } from './humanPerformanceRuntime'
import { TouchMotionSource } from './touchSource'

test('a completed tap keeps its visual tail but cannot be revived or prolonged by late events', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const tracker = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 0, region: 'hair' as const }
  source.update('panel', tracker.begin(sample)!, 0)
  const end = tracker.end({ ...sample, atMs: 60 })!
  source.update('panel', end, 60)
  const tail = source.current()
  const revision = source.version()
  t.mock.timers.tick(500)
  source.update('panel', { ...end, phase: 'update', gesture: 'hold', durationMs: 560 }, 560)
  source.refine('panel', source.version(), 'withdraw', 560)
  assert.equal(source.current(), tail)
  assert.equal(source.version(), revision)
  source.update('panel', end, 560)
  t.mock.timers.tick(600)
  assert.equal(source.current(), null, 'duplicate end must not extend the original release deadline')
})

test('hold-to-stroke keeps an applied reaction but rejects the old in-flight revision', () => {
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const tracker = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 0, region: 'hair' as const }
  source.update('panel', tracker.begin(sample)!, 0)
  source.update('panel', tracker.update({ ...sample, atMs: 500 })!, 500)
  const revision = source.version()
  source.refine('panel', revision, 'withdraw', 600)
  const before = source.current()!
  const stroke = tracker.update({ ...sample, x: 0.2, atMs: 700 })!
  assert.equal(stroke.gesture, 'stroke')
  source.update('panel', stroke, 700)
  assert.equal(source.current()!.behaviors[0].form.id, 'withdraw')
  assert.equal(source.current()!.id, before.id)
  assert.deepEqual(source.current()!.pegs, before.pegs)
  assert.notEqual(source.version(), revision)
  source.refine('panel', revision, 'accept', 800)
  assert.equal(source.current()!.behaviors[0].form.id, 'withdraw')
  source.update('panel', { ...stroke, durationMs: 900 }, 900)
  assert.equal(source.current()!.behaviors[0].form.id, 'withdraw')
  source.setAffect(20, 70, 950)
  assert.equal(source.current()!.behaviors[0].form.id, 'hesitate')
  source.refine('panel', source.version(), 'withdraw', 960)
  source.update('panel', { ...stroke, region: 'face', durationMs: 1000 }, 1000)
  assert.equal(source.current()!.behaviors[0].form.id, 'hesitate')
  source.release()
})

test('pressing again replaces the tap deadline without letting the old contact cancel the hold', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const tracker = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 0, region: 'hair' as const }
  source.update('panel', tracker.begin(sample)!, 0)
  const oldEnd = tracker.end({ ...sample, atMs: 60 })!
  source.update('panel', oldEnd, 60)
  const id = source.current()!.id
  t.mock.timers.tick(200)
  source.update('panel', tracker.begin({ ...sample, atMs: 260 })!, 260)
  source.update('panel', tracker.update({ ...sample, atMs: 760 })!, 760)
  source.update('panel', { ...oldEnd, phase: 'cancel' }, 770)
  t.mock.timers.tick(2000)
  assert.equal(source.current()!.id, id)
  assert.equal(source.current()!.behaviors[0].form.id, 'accept')
  source.update('panel', tracker.cancel(2760)!, 2760)
  assert.equal(source.current(), null)
})

test('rapid hair clicks preserve the displayed plan, start and intensity while invalidating old contact results', () => {
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const tracker = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 0, region: 'hair' as const }
  source.update('panel', tracker.begin(sample)!, 0)
  source.update('panel', tracker.end({ ...sample, atMs: 60 })!, 60)
  const first = source.current()!
  const oldRevision = source.version()
  for (let i = 1; i <= 8; i++) {
    source.update('panel', tracker.begin({ ...sample, atMs: i * 200 })!, i * 200)
    assert.equal(source.current()!.id, first.id)
    assert.equal(source.current()!.originMs, first.originMs)
    assert.equal(source.current()!.behaviors[0].intensity, 1)
    assert.equal(source.current()!.behaviors[0].form.id, 'accept')
    source.refine('panel', oldRevision, 'withdraw', i * 200)
    assert.equal(source.current()!.behaviors[0].form.id, 'accept')
    source.update('panel', tracker.end({ ...sample, atMs: i * 200 + 60 })!, i * 200 + 60)
  }
  source.release()
})

test('asynchronous appraisal reaches the realizer through the existing plan, without a second start', async () => {
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const tracker = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 0, region: 'hair' as const }
  source.update('panel', tracker.begin(sample)!, 0)
  const hold = tracker.update({ ...sample, atMs: 500 })!
  source.update('panel', hold, 500)
  const before = source.current()!
  const client = new TouchAppraisal({
    now: () => 600,
    request: async () => ({ reaction: 'withdraw' }),
    apply: (revision, reaction) => source.refine('panel', revision, reaction, 600),
  })
  client.observe(hold, source.version())
  await new Promise(resolve => setImmediate(resolve))
  const after = source.current()!
  assert.equal(after.id, before.id)
  assert.deepEqual(after.pegs, before.pegs)
  assert.equal(after.behaviors[0].form.id, 'withdraw')
  const scheduler = new HumanPerformanceRuntime()
  const realized = realizeAnime25DBehaviorPlan(scheduler.frame([after], 600).plan!, 600)
  assert.equal(realized.reports[0].result, 'accepted')
  client.dispose()
  source.release()
})

test('agent refinement survives pointer frames but not semantic changes or release', () => {
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const tracker = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 0, region: 'hair' as const }
  source.update('panel', tracker.begin(sample)!, 0)
  const hold = tracker.update({ ...sample, atMs: 500 })!
  source.update('panel', hold, 500)
  const revision = source.version()
  const origin = source.current()!.originMs
  source.refine('panel', revision, 'accept', 600)
  source.update('panel', { ...hold, durationMs: 700 }, 700)
  assert.equal(source.current()!.behaviors[0].form.id, 'accept')
  assert.equal(source.current()!.originMs, origin)
  source.update('panel', { ...hold, region: 'face' }, 800)
  source.refine('panel', revision, 'accept', 900)
  assert.equal(source.current()!.behaviors[0].form.id, 'hesitate')
  source.update('panel', { ...hold, region: 'accessory' }, 910)
  const accessoryRevision = source.version()
  source.update('panel', { ...hold, region: 'accessory' }, 920)
  assert.equal(source.version(), accessoryRevision, 'same pose in a new region must settle its semantic revision')
  source.release()
  source.refine('panel', source.version(), 'accept', 1000)
  assert.equal(source.current(), null)
})

test('contact is realized by the shared scheduler without claiming mouth or restarting on hold', () => {
  const coordinator = new RigMotionCoordinator()
  const speech = coordinator.claim('speech', ['mouth'])
  const music = coordinator.claim('music', ['headBody'])
  const source = new TouchMotionSource(coordinator, () => {})
  const gestures = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 1000, region: 'hair' as const }
  source.update('widget', gestures.begin(sample)!, 1000)
  const first = source.current()!
  const runtime = new HumanPerformanceRuntime()
  const initial = runtime.frame([first], 1000)
  assert.equal(realizeAnime25DBehaviorPlan(initial.plan!, 1000).reports[0].result, 'accepted')
  source.update('widget', gestures.update({ ...sample, atMs: 1500 })!, 1500)
  const hold = runtime.frame([source.current()], 1500)
  assert.equal(hold.plan!.behaviors[0].id, first.behaviors[0].id)
  assert.equal(source.current()!.originMs, 1000)
  assert.equal(coordinator.snapshot(1500).owners.mouth, 'speech')
  source.release('old-panel')
  assert.ok(source.current())
  source.update('widget', gestures.cancel(1600)!, 1600)
  assert.equal(source.current(), null)
  assert.equal(coordinator.snapshot(1600).owners.headBody, 'music')
  assert.equal(coordinator.snapshot(1600).owners.mouth, 'speech')
  coordinator.release(speech)
  coordinator.release(music)
})

test('motion intensity changes preserve identity; old owner cannot cancel a successor', () => {
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const gestures = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 0, region: 'face' as const }
  source.update('panel', gestures.begin(sample)!, 0)
  const id = source.current()!.behaviors[0].id
  source.update('panel', gestures.update({ ...sample, x: 0.2, atMs: 200 })!, 200)
  assert.equal(source.current()!.behaviors[0].id, id)
  assert.equal(source.current()!.behaviors[0].intensity, 0.95)
  assert.equal(source.current()!.behaviors[0].form.id, 'hesitate')
  const other = new TouchGestureTracker()
  source.update('widget', other.begin({ ...sample, atMs: 300 })!, 300)
  source.update('panel', gestures.cancel(400)!, 400)
  assert.ok(source.current()!.id.includes('widget'))
  source.release()
})

test('mood updates during contact revise the same behavior without resetting its start', () => {
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const gestures = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 1000, region: 'hair' as const }
  source.update('panel', gestures.begin(sample)!, 1000)
  source.update('panel', gestures.update({ ...sample, x: 0.2, atMs: 1400 })!, 1400)
  const previous = source.current()!
  assert.equal(previous.behaviors[0].form.id, 'accept')
  source.setAffect(30, 70, 1500)
  const revised = source.current()!
  assert.equal(revised.behaviors[0].form.id, 'hesitate')
  assert.equal(revised.behaviors[0].id, previous.behaviors[0].id)
  assert.deepEqual(revised.pegs, previous.pegs)
  const scheduler = new HumanPerformanceRuntime()
  scheduler.frame([previous], 1400)
  const frame = scheduler.frame([revised], 1500)
  const unit = realizeAnime25DBehaviorPlan(frame.plan!, 1500).units[0]
  assert.equal(unit.form, 'hesitate')
  assert.equal(unit.timing.startMs, 1000)
  source.release()
})

test('acceptance and hesitation reach the pose, revise continuously and release to rest', () => {
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const tracker = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 0, region: 'hair' as const }
  source.update('panel', tracker.begin(sample)!, 0)
  source.update('panel', { ...tracker.update({ ...sample, x: 0.2, atMs: 200 })!, position: { x: 0.8, y: -0.5 } }, 200)
  const expression = new PerformanceExpressionController()
  expression.playBehaviorUnits(realizeAnime25DBehaviorPlan(source.current()!, 200).units, 0.2, 200)
  for (let i = 12; i <= 60; i++) expression.sample(i / 60)
  const accepted = { ...expression.sample(1) }
  assert.ok(accepted.angleZ > 0.05)
  assert.ok(accepted.eyeOpen < 0)
  assert.equal(accepted.mouthForm, 0)
  source.setAffect(20, 70, 1000)
  expression.playBehaviorUnits(realizeAnime25DBehaviorPlan(source.current()!, 1000).units, 1, 1000)
  const transition = expression.sample(1 + 1 / 60)
  assert.ok(Math.abs(transition.angleZ - accepted.angleZ) < 0.06)
  for (let i = 62; i <= 120; i++) expression.sample(i / 60)
  assert.ok(expression.sample(2).angleZ < -0.02)
  expression.playBehaviorUnits([], 2, 2000)
  for (let i = 121; i <= 240; i++) expression.sample(i / 60)
  assert.ok(Math.abs(expression.sample(4).angleZ) < 0.001)
  source.release()
})
