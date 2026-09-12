import assert from 'node:assert/strict'
import test from 'node:test'
import { TouchGestureTracker } from '../interaction/touchGesture'
import { RigMotionCoordinator } from '../motion/coordinator'
import { TouchMotionSource } from '../motion/touchSource'
import { realizeAnime25DBehaviorPlan } from './behaviorRealizer'
import { PerformanceExpressionController } from './performanceExpression'
import { touchExpressionPatch } from './touchExpression'

function contact() {
  const coordinator = new RigMotionCoordinator()
  const source = new TouchMotionSource(coordinator, () => {})
  const tracker = new TouchGestureTracker()
  const sample = { pointerId: 1, x: 0, y: 0, atMs: 0, region: 'hair' as const }
  source.update('panel', { ...tracker.begin(sample)!, position: { x: 0.8, y: -0.5 } }, 0)
  return { source, tracker, sample, coordinator }
}

test('fast back-and-forth movement is not mistaken for gentle petting after direction smoothing', () => {
  const amount = (frequency: number) => {
    const { source, tracker, sample } = contact()
    for (let i = 1; i <= 480; i++) {
      const now = i * 1000 / 120
      source.update('panel', tracker.update({ ...sample, atMs: now, x: 0.14 * Math.sin(now / 1000 * frequency * 2 * Math.PI) })!, now)
    }
    const caress = realizeAnime25DBehaviorPlan(source.current()!, 4000).units[0].touch!.caress!
    source.release()
    return caress
  }
  const gentle = amount(0.3)
  assert.ok(gentle > 0.25)
  assert.ok(amount(8) < gentle * 0.25, 'cancelling opposite directions is not slow movement')
})

test('lifting stops directional input without removing the expression; changing region clears petting evidence', () => {
  const { source, tracker, sample } = contact()
  for (let i = 1; i <= 180; i++) {
    const now = i * 1000 / 60
    source.update('panel', tracker.update({ ...sample, atMs: now, x: now / 5000 })!, now)
  }
  const before = source.current()!
  const unit = realizeAnime25DBehaviorPlan(before, 3000).units[0]
  assert.ok(unit.touch!.strokeX > 0.1)
  assert.ok(unit.touch!.caress! > 0.25)
  const expression = new PerformanceExpressionController()
  expression.playBehaviorUnits([unit], 0, 0)
  for (let i = 0; i <= 180; i++) expression.sample(i / 60)
  const moving = { ...expression.sample(3.01) }
  source.update('panel', tracker.end({ ...sample, atMs: 3010, x: 0.6 })!, 3010)
  const lifted = realizeAnime25DBehaviorPlan(source.current()!, 3010).units[0]
  assert.equal(lifted.behaviorId, unit.behaviorId)
  assert.equal(lifted.touch!.strokeX, 0)
  assert.equal(lifted.touch!.strokeY, 0)
  assert.equal(lifted.touch!.caress, unit.touch!.caress)
  expression.playBehaviorUnits([lifted], 3.01, 3010)
  assert.equal(expression.sample(3.01).angleZ, moving.angleZ, 'removing direction must not snap the rendered head')
  const settling = expression.sample(3.01 + 1 / 60)
  assert.ok(settling.angleZ < moving.angleZ)
  assert.ok(Math.abs(settling.angleZ - moving.angleZ) < 0.01)
  source.update('panel', tracker.begin({ ...sample, atMs: 3100, x: 0.6 })!, 3100)
  source.update('panel', tracker.update({ ...sample, atMs: 3200, x: 0.61, region: 'face' })!, 3200)
  const face = realizeAnime25DBehaviorPlan(source.current()!, 3200).units[0]
  assert.equal(face.touch!.caress, 0)
  source.release()
})

test('petting builds from real movement, pauses resume the same phrase, and stale decisions cannot return', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { source, tracker, sample } = contact()
  const expression = new PerformanceExpressionController()
  let before = 0
  for (let i = 1; i <= 240; i++) {
    const now = i * 1000 / 60
    source.update('panel', tracker.update({ ...sample, atMs: now, x: 0.16 * Math.sin(now / 650) })!, now)
    expression.playBehaviorUnits(realizeAnime25DBehaviorPlan(source.current()!, now).units, now / 1000, now)
    before = expression.sample(now / 1000).eyeOpen
  }
  const plan = source.current()!
  const unit = realizeAnime25DBehaviorPlan(plan, 4000).units[0]
  assert.ok(unit.touch!.caress! > 0.25)
  assert.ok(before < -0.58)
  source.refine('panel', source.version(), 'withdraw', 4000)
  const oldRevision = source.version()
  source.update('panel', tracker.end({ ...sample, atMs: 4010, x: 0.16 * Math.sin(4000 / 650) })!, 4010)
  t.mock.timers.tick(150)
  source.update('panel', tracker.begin({ ...sample, atMs: 4160 })!, 4160)
  assert.equal(source.current()!.id, plan.id)
  assert.equal(source.current()!.originMs, plan.originMs)
  assert.equal(source.current()!.behaviors[0].form.id, 'withdraw')
  source.refine('panel', oldRevision, 'accept', 4170)
  assert.equal(source.current()!.behaviors[0].form.id, 'withdraw')
  t.mock.timers.tick(500)
  assert.ok(source.current(), 'the first release deadline must not end resumed petting')
  source.setAffect(20, 70, 4200)
  assert.equal(source.current()!.behaviors[0].form.id, 'hesitate')
  source.update('panel', tracker.cancel(4210)!, 4210)
  assert.equal(source.current(), null)
})

test('holding still cannot manufacture caress and a long pause starts a new phrase', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { source, tracker, sample } = contact()
  source.update('panel', tracker.update({ ...sample, atMs: 4000 })!, 4000)
  const old = source.current()!.id
  assert.equal(realizeAnime25DBehaviorPlan(source.current()!, 4000).units[0].touch!.caress, 0)
  source.update('panel', tracker.end({ ...sample, atMs: 4010 })!, 4010)
  t.mock.timers.tick(301)
  assert.equal(source.current(), null)
  source.update('panel', tracker.begin({ ...sample, atMs: 4400 })!, 4400)
  assert.notEqual(source.current()!.id, old)
  assert.equal(source.current()!.behaviors[0].form.id, 'notice')
  source.release()
})

test('30/60/120 fps petting and a short lift keep the rendered eyes continuous without replaying the nod', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const settled = []
  for (const fps of [30, 60, 120]) {
    const { source, tracker, sample } = contact()
    const expression = new PerformanceExpressionController()
    let lastEye = 0
    let maxStep = 0
    for (let i = 1; i <= fps * 6; i++) {
      const now = i * 1000 / fps
      const point = { ...sample, atMs: now, x: 0.16 * Math.sin(now / 650) }
      let observation = null
      if (i === fps * 4) observation = tracker.end(point)
      else if (i === fps * 4 + Math.round(fps * 0.2)) observation = tracker.begin(point)
      else observation = tracker.update(point)
      if (observation) source.update('panel', observation, now)
      const units = realizeAnime25DBehaviorPlan(source.current()!, now).units
      expression.playBehaviorUnits(units, now / 1000, now)
      const pose = expression.sample(now / 1000)
      if (i >= fps * 4 && i < fps * 5) {
        maxStep = Math.max(maxStep, Math.abs(pose.eyeOpen - lastEye))
        assert.ok(pose.eyeOpen < -0.56, 'lifting the hand must not flash the eyes open')
        assert.ok(Math.abs(pose.angleY) < 0.005, 'resuming must not restart the nod')
      }
      lastEye = pose.eyeOpen
    }
    assert.ok(maxStep < 0.015, `${fps}: eyelid step ${maxStep}`)
    settled.push(lastEye)
    source.release()
  }
  assert.ok(Math.max(...settled) - Math.min(...settled) < 0.015)
})

test('caress changes acceptance only, never softens refusal or writes special eye art', () => {
  const contact = { x: 0.3, y: 0, strokeX: 0.2, strokeY: 0, caress: 0.7 }
  const plain = { ...contact, caress: 0 }
  for (const form of ['notice', 'hesitate', 'withdraw'] as const) {
    assert.deepEqual(touchExpressionPatch(form, 1, contact, 3), touchExpressionPatch(form, 1, plain, 3))
  }
  const soft = touchExpressionPatch('accept', 1, contact, 3)
  const tap = touchExpressionPatch('accept', 1, plain, 3)
  assert.ok(soft.eyeOpen! < tap.eyeOpen!)
  assert.ok(soft.body! > tap.body!)
  assert.equal(soft.eyeSqueeze, undefined)
  assert.equal(soft.mouthOpen, undefined)
})

test('response phrase pairs expression with a single head answer and settles during a long hold', () => {
  const center = { x: 0, y: 0, strokeX: 0, strokeY: 0 }
  const early = touchExpressionPatch('accept', 1, center, 0.05)
  const answer = touchExpressionPatch('accept', 1, center, 0.5)
  const late = touchExpressionPatch('accept', 1, center, 6)
  assert.equal(early.angleY, 0)
  assert.ok(answer.angleY! > 0.1)
  assert.ok(answer.eyeOpen! < -0.45)
  assert.ok(Math.abs(late.angleY!) < 0.001)
  assert.ok(late.eyeOpen! < -0.45, 'hold keeps the expression, not the nod')
  const retreat = touchExpressionPatch('withdraw', 1, center, 0.5)
  assert.ok(retreat.angleY! < -0.2)
  assert.ok(retreat.browAngSym! > 0.7)
})

test('late appraisal answers from the current pose; starting a stroke preserves its face and phrase', () => {
  const { source, tracker, sample } = contact()
  const expression = new PerformanceExpressionController()
  source.update('panel', { ...tracker.update({ ...sample, atMs: 500 })!, position: { x: 0, y: 0 } }, 500)
  expression.playBehaviorUnits(realizeAnime25DBehaviorPlan(source.current()!, 500).units, 0.5, 500)
  for (let i = 30; i <= 240; i++) expression.sample(i / 60)
  source.refine('panel', source.version(), 'withdraw', 4000)
  const before = { ...expression.sample(4) }
  expression.playBehaviorUnits(realizeAnime25DBehaviorPlan(source.current()!, 4000).units, 4, 4000)
  assert.equal(expression.sample(4).angleY, before.angleY)
  for (let i = 241; i <= 270; i++) expression.sample(i / 60)
  const peak = expression.sample(4.5).angleY
  assert.ok(peak < -0.16)
  for (let i = 271; i <= 480; i++) {
    const now = i * 1000 / 60
    const stroke = tracker.update({ ...sample, x: 0.2, atMs: now })!
    assert.equal(stroke.gesture, 'stroke')
    source.update('panel', { ...stroke, position: { x: 0, y: Math.sin(i) * 0.05 } }, now)
    expression.playBehaviorUnits(realizeAnime25DBehaviorPlan(source.current()!, now).units, i / 60, now)
    assert.ok(expression.sample(i / 60).browAngSym > 0.65, 'moving the finger must not restore the happy brows')
  }
  assert.ok(expression.sample(8).angleY > peak + 0.03)
  source.release()
})

test('left/right contacts mirror gaze and lean; withdrawal moves away while watching the contact', () => {
  for (const form of ['notice', 'accept', 'hesitate', 'withdraw'] as const) {
    const right = touchExpressionPatch(form, 1, { x: 0.8, y: 0, strokeX: 0.4, strokeY: 0 }, 1)
    const left = touchExpressionPatch(form, 1, { x: -0.8, y: 0, strokeX: -0.4, strokeY: 0 }, 1)
    assert.equal(right.eyeX, -left.eyeX!)
    assert.equal(right.angleZ, -left.angleZ!)
    assert.ok(right.eyeX! > 0)
    assert.equal(Math.sign(right.angleZ!), form === 'withdraw' || form === 'hesitate' ? -1 : 1)
  }
  const still = touchExpressionPatch('accept', 1, { x: 0, y: 0, strokeX: 0, strokeY: 0 })
  assert.equal(still.angleZ, 0, 'a centered contact must not invent a favored side')
})

test('spatial revisions reach the realizer without resetting time or invalidating model appraisal', () => {
  const { source, tracker, sample } = contact()
  const version = source.version()
  const origin = source.current()!.originMs
  source.update('panel', { ...tracker.update({ ...sample, x: 0.02, atMs: 50 })!, position: { x: -0.8, y: 0.3 } }, 50)
  assert.equal(source.version(), version)
  assert.equal(source.current()!.originMs, origin)
  const unit = realizeAnime25DBehaviorPlan(source.current()!, 50).units[0]
  assert.equal(unit.touch?.x, -0.8)
  assert.equal(unit.touch?.y, 0.3)
  assert.ok(unit.touch!.strokeX > 0)
  source.release()
})

test('30/60/120 fps preserve settling and a bounded smooth release from the displayed pose', () => {
  const results = []
  for (const fps of [30, 60, 120]) {
    const { source, coordinator } = contact()
    const music = coordinator.claim('music', ['headBody'])
    const speech = coordinator.claim('speech', ['mouth'])
    const expression = new PerformanceExpressionController()
    expression.playBehaviorUnits(realizeAnime25DBehaviorPlan(source.current()!, 0).units, 0, 0)
    let early = 0
    for (let i = 0; i <= fps * 5; i++) {
      const output = expression.sample(i / fps)
      if (i === fps) early = output.angleZ
    }
    const held = { ...expression.sample(5) }
    assert.ok(held.angleZ < early, 'one orienting response must settle, not stay frozen')
    source.release()
    assert.equal(coordinator.snapshot(5000).owners.headBody, 'music')
    assert.equal(coordinator.snapshot(5000).owners.mouth, 'speech')
    expression.playBehaviorUnits([], 5, 5000)
    assert.equal(expression.sample(5).angleZ, held.angleZ)
    const first = expression.sample(5 + 1 / fps).angleZ
    assert.ok(Math.abs(first - held.angleZ) < 0.002, `${fps}: release must not snap`)
    let last = first
    for (let i = 2; i <= fps; i++) {
      const next = expression.sample(5 + i / fps).angleZ
      assert.ok(next <= last + 1e-9 && next >= 0)
      last = next
    }
    assert.equal(last, 0)
    results.push(held.angleZ)
    coordinator.release(music); coordinator.release(speech)
  }
  assert.ok(Math.max(...results) - Math.min(...results) < 0.001)
})

test('fast direction reversal is continuous and starts responding on the first sampled frame', () => {
  const { source, tracker, sample } = contact()
  const expression = new PerformanceExpressionController()
  expression.playBehaviorUnits(realizeAnime25DBehaviorPlan(source.current()!, 0).units, 0, 0)
  for (let i = 0; i <= 60; i++) expression.sample(i / 60)
  const before = { ...expression.sample(1) }
  source.update('panel', { ...tracker.update({ ...sample, atMs: 1000 })!, position: { x: -0.8, y: -0.5 } }, 1000)
  expression.playBehaviorUnits(realizeAnime25DBehaviorPlan(source.current()!, 1000).units, 1, 1000)
  assert.equal(expression.sample(1).angleZ, before.angleZ)
  const next = { ...expression.sample(1 + 1 / 60) }
  assert.ok(next.eyeX < before.eyeX)
  assert.ok(next.angleZ < before.angleZ)
  assert.ok(Math.abs(next.angleZ - before.angleZ) < 0.04)
  assert.ok((before.eyeX - next.eyeX) / before.eyeX > (before.angleZ - next.angleZ) / before.angleZ)
  source.release()
})

test('rapid repeated taps have bounded overlap and no residual pose after the final release', () => {
  const source = new TouchMotionSource(new RigMotionCoordinator(), () => {})
  const tracker = new TouchGestureTracker()
  const expression = new PerformanceExpressionController()
  let peak = 0
  for (let frame = 0; frame <= 240; frame++) {
    const atMs = frame * 1000 / 60
    const sample = { pointerId: 1, x: 0, y: 0, atMs, region: 'face' as const }
    const observation = frame % 6 === 0 ? tracker.begin(sample) : frame % 6 === 2 ? tracker.end(sample) : null
    if (observation) source.update('panel', { ...observation, position: { x: 0.8, y: 0 } }, atMs)
    const plan = source.current()
    expression.playBehaviorUnits(plan ? realizeAnime25DBehaviorPlan(plan, atMs).units : [], atMs / 1000, atMs)
    const output = expression.sample(atMs / 1000)
    peak = Math.max(peak, Math.abs(output.angleZ), Math.abs(output.body))
    assert.equal(output.mouthForm, 0)
  }
  assert.ok(peak < 0.45, `repeated taps must not accumulate unbounded energy: ${peak}`)
  source.release()
  expression.playBehaviorUnits([], 4, 4000)
  for (let frame = 241; frame <= 300; frame++) expression.sample(frame / 60)
  assert.equal(expression.sample(5).body, 0)
})
